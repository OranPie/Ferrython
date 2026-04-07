# Phase 150: __getattribute__ with super(), __sizeof__, chr() surrogates

# Test 1: __getattribute__ with super().__getattribute__()
class Intercept:
    def __init__(self):
        self.data = {'x': 42}
    def __getattribute__(self, name):
        return super().__getattribute__(name)
    def __getattr__(self, name):
        if name in self.data:
            return self.data[name]
        raise AttributeError(name)

i = Intercept()
assert i.data == {'x': 42}, f"Expected dict, got {i.data}"
assert i.x == 42, f"Expected 42 via __getattr__, got {i.x}"

# Test __getattribute__ doesn't infinite-recurse
class Counter:
    def __init__(self):
        self._count = 0
    def __getattribute__(self, name):
        return super().__getattribute__(name)

c = Counter()
assert c._count == 0

# Test 2: __sizeof__ on builtin types
assert isinstance((42).__sizeof__(), int), "__sizeof__ should return int"
assert (42).__sizeof__() > 0, "__sizeof__ should be positive"
assert "hello".__sizeof__() > 0
assert [1,2,3].__sizeof__() > 0
assert {'a': 1}.__sizeof__() > 0
assert (1, 2).__sizeof__() > 0

# Test 3: chr() with surrogates (0xD800-0xDFFF)
result = chr(0xD800)
assert isinstance(result, str), f"chr(0xD800) should return str, got {type(result)}"
assert len(result) == 1, f"chr(0xD800) should be 1 char, got {len(result)}"
# Normal chars still work
assert chr(65) == 'A'
assert chr(0x1F600) == '😀'

# Test 4: __getattr__ without __getattribute__ (should work as fallback)
class Proxy:
    def __init__(self):
        self._data = {'x': 42, 'y': 99}
    def __getattr__(self, name):
        if name.startswith('_'):
            raise AttributeError(name)
        return self._data.get(name, 'default')

p = Proxy()
assert p.x == 42
assert p.y == 99
assert p.z == 'default'

# Test 5: __setattr__ with super().__setattr__()
class Validated:
    def __setattr__(self, name, value):
        if name == 'age' and (not isinstance(value, int) or value < 0):
            raise ValueError("age must be positive int")
        super().__setattr__(name, value)

v = Validated()
v.age = 25
assert v.age == 25
v.name = 'Alice'
assert v.name == 'Alice'
try:
    v.age = -1
    assert False, "should have raised ValueError"
except ValueError:
    pass

print("All phase 150 tests passed!")
