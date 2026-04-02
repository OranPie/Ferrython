"""Phase 54: iter sentinel, object.__setattr__, __reversed__, format spec fixes"""
passed = 0
failed = 0
def test(name, cond):
    global passed, failed
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# === iter(callable, sentinel) ===
class Counter:
    def __init__(self):
        self.n = 0
    def __call__(self):
        self.n += 1
        return self.n

ct = Counter()
result = list(iter(ct, 4))
test("iter sentinel basic", result == [1, 2, 3])

ct2 = Counter()
result2 = list(iter(ct2, 1))
test("iter sentinel first", result2 == [])

ct3 = Counter()
result3 = list(iter(ct3, 6))
test("iter sentinel 6", result3 == [1, 2, 3, 4, 5])

# === object.__setattr__ ===
class Validated:
    def __init__(self, value):
        object.__setattr__(self, '_value', value)
    def __setattr__(self, name, value):
        if name == '_value' and isinstance(value, int) and value < 0:
            raise ValueError("negative")
        object.__setattr__(self, name, value)

v = Validated(42)
test("obj setattr init", v._value == 42)
v._value = 10
test("obj setattr update", v._value == 10)
try:
    v._value = -5
    test("obj setattr validate", False)
except ValueError:
    test("obj setattr validate", True)

# === object.__getattribute__ ===
test("obj getattr", object.__getattribute__(v, '_value') == 10)

# === object.__delattr__ ===
class Deletable:
    def __init__(self):
        self.x = 1
        self.y = 2

d = Deletable()
object.__delattr__(d, 'x')
test("obj delattr", not hasattr(d, 'x'))
test("obj delattr keeps", d.y == 2)

# === __reversed__ custom protocol ===
class MyList:
    def __init__(self, data):
        self.data = data
    def __reversed__(self):
        return iter(self.data[::-1])

ml = MyList([1, 2, 3, 4, 5])
test("custom reversed", list(reversed(ml)) == [5, 4, 3, 2, 1])

# Builtin reversed
test("reversed list", list(reversed([10, 20, 30])) == [30, 20, 10])
test("reversed str", list(reversed("abc")) == ["c", "b", "a"])
test("reversed tuple", list(reversed((1, 2, 3))) == [3, 2, 1])
test("reversed range", list(reversed(range(4))) == [3, 2, 1, 0])

# === __reversed__ with generator ===
class FancyReverse:
    def __init__(self, items):
        self.items = items
    def __reversed__(self):
        for i in range(len(self.items) - 1, -1, -1):
            yield self.items[i]

fr = FancyReverse([10, 20, 30])
test("reversed generator", list(reversed(fr)) == [30, 20, 10])

# === Format spec #alternate form ===
test("hex alt", f"{255:#x}" == "0xff")
test("oct alt", f"{8:#o}" == "0o10")
test("bin alt", f"{10:#b}" == "0b1010")
test("HEX alt", f"{255:#X}" == "0XFF")
test("hex no alt", f"{255:x}" == "ff")
test("bin pad", f"{42:08b}" == "00101010")

# === Comma thousands separator ===
test("comma f", f"{1234.5:,.2f}" == "1,234.50")
test("comma f big", f"{1234567.89:,.2f}" == "1,234,567.89")

# === Reverse operators ===
class MyNum:
    def __init__(self, v):
        self.v = v
    def __radd__(self, other):
        return MyNum(other + self.v)
    def __rsub__(self, other):
        return MyNum(other - self.v)
    def __rmul__(self, other):
        return MyNum(other * self.v)
    def __rtruediv__(self, other):
        return MyNum(other / self.v)

n = MyNum(10)
test("radd", (5 + n).v == 15)
test("rsub", (20 - n).v == 10)
test("rmul", (3 * n).v == 30)
test("rtruediv", (100 / n).v == 10.0)

# === __index__ protocol ===
class Idx:
    def __init__(self, i):
        self.i = i
    def __index__(self):
        return self.i

lst = [10, 20, 30, 40, 50]
idx = Idx(2)
test("index proto list", lst[idx] == 30)
test("index proto neg", lst[Idx(-1)] == 50)

# === Multiple return + unpack ===
def minmax(items):
    return min(items), max(items)
lo, hi = minmax([3, 1, 4, 1, 5, 9])
test("multi return", lo == 1 and hi == 9)

# === Complex comprehension patterns ===
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat = [x for row in matrix for x in row if x % 2 == 0]
test("nested comp filter", flat == [2, 4, 6, 8])

# Comprehension with walrus
nums = [1, 5, 3, 8, 2, 7]
big = [(n, sq) for n in nums if (sq := n * n) > 10]
test("walrus comp", big == [(5, 25), (8, 64), (7, 49)])

# Dict comprehension
squares = {x: x**2 for x in range(6)}
test("dict comp", squares == {0: 0, 1: 1, 2: 4, 3: 9, 4: 16, 5: 25})

# Set comprehension
evens = {x for x in range(10) if x % 2 == 0}
test("set comp", evens == {0, 2, 4, 6, 8})

# === Generator pipeline ===
def gen_range(n):
    for i in range(n):
        yield i

def gen_square(source):
    for x in source:
        yield x * x

def gen_filter_big(source, threshold):
    for x in source:
        if x > threshold:
            yield x

pipeline = list(gen_filter_big(gen_square(gen_range(10)), 20))
test("gen pipeline", pipeline == [25, 36, 49, 64, 81])

# === Chained comparison ===
x = 5
test("chain 3", 1 < x < 10)
test("chain false", not (1 < x > 10))
test("chain 4", 0 <= x <= 5 <= 10)

# === MRO super chain (diamond) ===
class A:
    def who(self):
        return ["A"]
class B(A):
    def who(self):
        return ["B"] + super().who()
class C(A):
    def who(self):
        return ["C"] + super().who()
class D(B, C):
    def who(self):
        return ["D"] + super().who()

test("mro chain", D().who() == ["D", "B", "C", "A"])

# === Property with validation ===
class Temperature:
    def __init__(self, celsius):
        self._celsius = celsius
    
    @property
    def celsius(self):
        return self._celsius
    
    @celsius.setter
    def celsius(self, value):
        if value < -273.15:
            raise ValueError("below absolute zero")
        self._celsius = value
    
    @property
    def fahrenheit(self):
        return self._celsius * 9/5 + 32

t = Temperature(100)
test("prop get", t.celsius == 100)
test("prop computed", t.fahrenheit == 212.0)
t.celsius = 0
test("prop set", t.celsius == 0)
try:
    t.celsius = -300
    test("prop validate", False)
except ValueError:
    test("prop validate", True)

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 54 TESTS PASSED")
