"""Phase 51: Advanced patterns — decorator factories, context managers,
   exception chaining, complex generators, multiple return paths"""

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

# 1. Decorator factory with arguments
def repeat(n):
    def decorator(func):
        def wrapper(*args, **kwargs):
            results = []
            for _ in range(n):
                results.append(func(*args, **kwargs))
            return results
        return wrapper
    return decorator

@repeat(3)
def greet(name):
    return f"Hello, {name}!"

test("decorator factory", greet("World") == ["Hello, World!"] * 3)

# 2. Chained decorators
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
def styled(text):
    return text

test("chained decorators", styled("hello") == "<b><i>hello</i></b>")

# 3. Context manager with exception suppression
class Suppress:
    def __init__(self, *exceptions):
        self.exceptions = exceptions
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is not None:
            for exc in self.exceptions:
                if exc_type == exc or (isinstance(exc_type, type) and issubclass(exc_type, exc)):
                    return True
        return False

with Suppress(ValueError, TypeError):
    x = int("not a number")

test("suppress exception", True)  # If we get here, exception was suppressed

# 4. Context manager exception not suppressed
caught_outer = False
try:
    with Suppress(ValueError):
        raise KeyError("wrong key")
except KeyError:
    caught_outer = True

test("not suppressed", caught_outer)

# 5. Generator as pipeline
def integers(n):
    for i in range(n):
        yield i

def squared(gen):
    for x in gen:
        yield x * x

def filtered(gen, pred):
    for x in gen:
        if pred(x):
            yield x

pipeline = list(filtered(squared(integers(10)), lambda x: x > 20))
test("gen pipeline", pipeline == [25, 36, 49, 64, 81])

# 6. Generator send
def accumulator():
    total = 0
    while True:
        value = yield total
        if value is None:
            break
        total += value

gen = accumulator()
next(gen)  # Prime the generator
gen.send(10)
gen.send(20)
result = gen.send(30)
test("gen send", result == 60)

# 7. Complex dict patterns
def invert_dict(d):
    return {v: k for k, v in d.items()}

test("invert dict", invert_dict({"a": 1, "b": 2}) == {1: "a", 2: "b"})

# 8. Nested comprehensions
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat = [x for row in matrix for x in row]
test("nested comp flat", flat == [1, 2, 3, 4, 5, 6, 7, 8, 9])

transpose = [[row[i] for row in matrix] for i in range(3)]
test("transpose", transpose == [[1, 4, 7], [2, 5, 8], [3, 6, 9]])

# 9. Default dict pattern
def word_frequency(text):
    freq = {}
    for word in text.split():
        freq[word] = freq.get(word, 0) + 1
    return freq

test("word freq", word_frequency("the cat sat on the mat") == 
     {"the": 2, "cat": 1, "sat": 1, "on": 1, "mat": 1})

# 10. Complex class hierarchy with super()
class Animal:
    def __init__(self, name):
        self.name = name
    def speak(self):
        return f"{self.name} says ..."

class Dog(Animal):
    def speak(self):
        return f"{self.name} says Woof!"

class Cat(Animal):
    def speak(self):
        return f"{self.name} says Meow!"

class PoliceDog(Dog):
    def speak(self):
        return super().speak() + " I'm a police dog!"

test("animal dog", Dog("Rex").speak() == "Rex says Woof!")
test("police dog", PoliceDog("K9").speak() == "K9 says Woof! I'm a police dog!")

# 11. Method chaining
class StringBuilder:
    def __init__(self):
        self.parts = []
    def add(self, text):
        self.parts.append(text)
        return self
    def build(self):
        return "".join(self.parts)

result = StringBuilder().add("Hello").add(", ").add("World!").build()
test("method chain", result == "Hello, World!")

# 12. Factory method pattern
class Shape:
    @staticmethod
    def create(kind, **kwargs):
        if kind == "circle":
            return {"type": "circle", "radius": kwargs.get("radius", 1)}
        elif kind == "rect":
            return {"type": "rect", "w": kwargs.get("w", 1), "h": kwargs.get("h", 1)}
        return None

c = Shape.create("circle", radius=5)
test("factory circle", c["type"] == "circle" and c["radius"] == 5)
r = Shape.create("rect", w=3, h=4)
test("factory rect", r["type"] == "rect" and r["w"] == 3)

# 13. Multiple return values
def divmod_custom(a, b):
    return a // b, a % b

q, r = divmod_custom(17, 5)
test("multi return", q == 3 and r == 2)

# 14. Recursive fibonacci with memoization
def fib_memo(n, memo={}):
    if n in memo:
        return memo[n]
    if n <= 1:
        return n
    memo[n] = fib_memo(n-1, memo) + fib_memo(n-2, memo)
    return memo[n]

test("fib memo", fib_memo(30) == 832040)

# 15. Complex sorting
data = [
    {"name": "Charlie", "age": 35},
    {"name": "Alice", "age": 30},
    {"name": "Bob", "age": 25},
]
by_name = sorted(data, key=lambda x: x["name"])
test("sort by name", [d["name"] for d in by_name] == ["Alice", "Bob", "Charlie"])

by_age = sorted(data, key=lambda x: x["age"])
test("sort by age", [d["name"] for d in by_age] == ["Bob", "Alice", "Charlie"])

# 16. String template
def format_template(template, **kwargs):
    result = template
    for key, value in kwargs.items():
        result = result.replace(f"{{{key}}}", str(value))
    return result

test("template", format_template("Hello, {name}! You are {age}.", name="Alice", age=30) 
     == "Hello, Alice! You are 30.")

# 17. Zip with dict creation
keys = ["name", "age", "city"]
values = ["Alice", 30, "NYC"]
result = dict(zip(keys, values))
test("zip dict", result == {"name": "Alice", "age": 30, "city": "NYC"})

# 18. Complex iteration
def pairwise(iterable):
    items = list(iterable)
    return [(items[i], items[i+1]) for i in range(len(items) - 1)]

test("pairwise", pairwise([1, 2, 3, 4]) == [(1, 2), (2, 3), (3, 4)])

# 19. Recursive tree traversal
def count_nodes(tree):
    if tree is None:
        return 0
    count = 1
    for child in tree.get("children", []):
        count += count_nodes(child)
    return count

tree = {
    "value": 1,
    "children": [
        {"value": 2, "children": [
            {"value": 4, "children": []},
            {"value": 5, "children": []}
        ]},
        {"value": 3, "children": [
            {"value": 6, "children": []}
        ]}
    ]
}
test("tree count", count_nodes(tree) == 6)

# 20. Set operations
a = {1, 2, 3, 4, 5}
b = {4, 5, 6, 7, 8}
test("set union", a | b == {1, 2, 3, 4, 5, 6, 7, 8})
test("set intersect", a & b == {4, 5})
test("set diff", a - b == {1, 2, 3})
test("set sym diff", a ^ b == {1, 2, 3, 6, 7, 8})

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 51 TESTS PASSED")
