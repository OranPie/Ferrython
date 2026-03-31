# ═══════════════════════════════════════════
# Phase 3 Tests — File I/O, Decorators, From-Import,
# Advanced Classes, Edge Cases
# ═══════════════════════════════════════════

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL: " + name)

# ── Property Decorator ──

def test_property():
    class Temperature:
        def __init__(self, celsius):
            self._celsius = celsius
        
        @property
        def celsius(self):
            return self._celsius
        
        @property
        def fahrenheit(self):
            return self._celsius * 9 / 5 + 32
    
    t = Temperature(100)
    test("property_get", t.celsius == 100)
    test("property_computed", t.fahrenheit == 212.0)

def test_staticmethod():
    class MathUtils:
        @staticmethod
        def add(a, b):
            return a + b
        
        @staticmethod
        def multiply(a, b):
            return a * b
    
    test("staticmethod_call", MathUtils.add(3, 4) == 7)
    test("staticmethod_multiply", MathUtils.multiply(5, 6) == 30)
    
    m = MathUtils()
    test("staticmethod_instance", m.add(10, 20) == 30)

def test_classmethod():
    class Counter:
        count = 0
        
        @classmethod
        def increment(cls):
            cls.count = cls.count + 1
            return cls.count
    
    test("classmethod_1", Counter.increment() == 1)
    test("classmethod_2", Counter.increment() == 2)
    test("classmethod_3", Counter.count == 2)

# ── From-Import ──

def test_from_import():
    from math import sqrt, pi, ceil, floor
    test("from_import_sqrt", sqrt(16) == 4.0)
    test("from_import_pi", abs(pi - 3.14159265) < 0.001)
    test("from_import_ceil", ceil(1.1) == 2)
    test("from_import_floor", floor(1.9) == 1)

def test_from_import_json():
    from json import dumps, loads
    s = dumps([1, 2, 3])
    test("from_import_json_dumps", s == "[1, 2, 3]")
    obj = loads('{"key": "value"}')
    test("from_import_json_loads", obj["key"] == "value")

# ── File I/O ──

def test_file_write_read():
    f = open("/tmp/ferrython_phase3.txt", "w")
    f.write("Hello\nWorld\nTest\n")
    f.close()
    
    f = open("/tmp/ferrython_phase3.txt", "r")
    content = f.read()
    f.close()
    test("file_write_read", "Hello" in content and "World" in content)

def test_file_readline():
    f = open("/tmp/ferrython_phase3.txt", "r")
    line1 = f.readline()
    line2 = f.readline()
    f.close()
    test("file_readline_1", "Hello" in line1)
    test("file_readline_2", "World" in line2)

def test_file_readlines():
    f = open("/tmp/ferrython_phase3.txt", "r")
    lines = f.readlines()
    f.close()
    test("file_readlines", len(lines) >= 3)

# ── Advanced Star Unpacking ──

def test_star_string():
    a, *b = "hello"
    test("star_unpack_string", a == "h" and b == ["e", "l", "l", "o"])

def test_star_in_loop():
    results = []
    data = [(1, 2, 3), (4, 5, 6)]
    for item in data:
        results.append(item[0])
    test("star_in_loop", results == [1, 4])

# ── F-String Advanced ──

def test_fstring_method():
    name = "world"
    test("fstring_method", f"{name.upper()}" == "WORLD")

def test_fstring_conditional():
    x = 5
    test("fstring_conditional", f"{'even' if x % 2 == 0 else 'odd'}" == "odd")

def test_fstring_nested_quotes():
    items = ["a", "b", "c"]
    test("fstring_list_join", f"{', '.join(items)}" == "a, b, c")

# ── Generator Advanced ──

def test_generator_stateful():
    def counter(start=0):
        n = start
        while True:
            yield n
            n = n + 1
    
    c = counter(10)
    test("gen_stateful_1", next(c) == 10)
    test("gen_stateful_2", next(c) == 11)
    test("gen_stateful_3", next(c) == 12)

def test_generator_chain():
    def evens(n):
        for i in range(n):
            if i % 2 == 0:
                yield i
    
    result = list(evens(10))
    test("gen_chain_evens", result == [0, 2, 4, 6, 8])

def test_generator_multiple():
    def gen1():
        yield 1
        yield 2
    def gen2():
        yield 3
        yield 4
    r1 = list(gen1())
    r2 = list(gen2())
    test("gen_multiple", r1 + r2 == [1, 2, 3, 4])

# ── Complex Exception Handling ──

def test_except_hierarchy():
    caught = None
    try:
        x = {}["missing"]
    except LookupError:
        caught = "lookup"
    test("except_hierarchy", caught == "lookup")

def test_except_multiple():
    caught = None
    try:
        x = 1 / 0
    except (ValueError, ZeroDivisionError):
        caught = "caught"
    test("except_multiple_types", caught == "caught")

def test_nested_try():
    result = []
    try:
        try:
            raise ValueError("inner")
        except ValueError:
            result.append("inner_caught")
            raise TypeError("outer")
    except TypeError:
        result.append("outer_caught")
    test("nested_try", result == ["inner_caught", "outer_caught"])

# ── Advanced Class Features ──

def test_class_str():
    class Point:
        def __init__(self, x, y):
            self.x = x
            self.y = y
        def __str__(self):
            return f"Point({self.x}, {self.y})"
    
    p = Point(3, 4)
    test("class_str", str(p) == "Point(3, 4)")

def test_class_repr():
    class Pair:
        def __init__(self, a, b):
            self.a = a
            self.b = b
        def __repr__(self):
            return f"Pair({self.a!r}, {self.b!r})"
    
    p = Pair("x", "y")
    test("class_repr", repr(p) == "Pair('x', 'y')")

def test_class_len():
    class Container:
        def __init__(self):
            self.items = []
        def add(self, item):
            self.items.append(item)
        def __len__(self):
            return len(self.items)
    
    c = Container()
    c.add(1)
    c.add(2)
    c.add(3)
    test("class_len", len(c) == 3)

def test_class_bool():
    class Maybe:
        def __init__(self, value):
            self.value = value
        def __bool__(self):
            return self.value is not None
    
    test("class_bool_true", bool(Maybe(42)) == True)
    test("class_bool_false", bool(Maybe(None)) == False)

# ── Math Module Advanced ──

def test_math_trig():
    import math
    test("math_sin_zero", abs(math.sin(0)) < 0.0001)
    test("math_cos_zero", abs(math.cos(0) - 1.0) < 0.0001)
    test("math_tan_zero", abs(math.tan(0)) < 0.0001)
    test("math_degrees", abs(math.degrees(math.pi) - 180.0) < 0.0001)
    test("math_radians", abs(math.radians(180) - math.pi) < 0.0001)

def test_math_special():
    import math
    test("math_e", abs(math.e - 2.71828) < 0.001)
    test("math_tau", abs(math.tau - 6.28318) < 0.001)
    test("math_inf", math.inf > 1000000)
    test("math_isfinite", math.isfinite(1.0) == True)
    test("math_isfinite_inf", math.isfinite(math.inf) == False)

# ── JSON Module Advanced ──

def test_json_nested():
    import json
    data = {"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}
    s = json.dumps(data)
    parsed = json.loads(s)
    test("json_nested_roundtrip", parsed["users"][0]["name"] == "Alice")
    test("json_nested_age", parsed["users"][1]["age"] == 25)

def test_json_types():
    import json
    test("json_true", json.loads("true") == True)
    test("json_false", json.loads("false") == False)
    test("json_null", json.loads("null") == None)
    test("json_float", json.loads("3.14") == 3.14)
    test("json_negative", json.loads("-42") == -42)
    test("json_string", json.loads('"hello"') == "hello")

# ── Cleanup ──

def test_cleanup():
    import os
    try:
        os.remove("/tmp/ferrython_phase3.txt")
    except:
        pass
    test("cleanup", True)

# ── Run all tests ──

test_property()
test_staticmethod()
test_classmethod()
test_from_import()
test_from_import_json()
test_file_write_read()
test_file_readline()
test_file_readlines()
test_star_string()
test_star_in_loop()
test_fstring_method()
test_fstring_conditional()
test_fstring_nested_quotes()
test_generator_stateful()
test_generator_chain()
test_generator_multiple()
test_except_hierarchy()
test_except_multiple()
test_nested_try()
test_class_str()
test_class_repr()
test_class_len()
test_class_bool()
test_math_trig()
test_math_special()
test_json_nested()
test_json_types()
test_cleanup()

print("========================================")
print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(f"SOME TESTS FAILED: {failed}")
