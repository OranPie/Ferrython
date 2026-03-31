# ═══════════════════════════════════════════
# Phase 2 Tests — Generators, With, F-strings, 
# Star Unpacking, Imports, Modules
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

# ── Generators ──

def test_generator_basic():
    def gen():
        yield 1
        yield 2
        yield 3
    result = list(gen())
    test("generator_basic", result == [1, 2, 3])

def test_generator_range():
    def countdown(n):
        while n > 0:
            yield n
            n = n - 1
    test("generator_countdown", list(countdown(5)) == [5, 4, 3, 2, 1])

def test_generator_fibonacci():
    def fib(n):
        a, b = 0, 1
        for i in range(n):
            yield a
            a, b = b, a + b
    test("generator_fibonacci", list(fib(8)) == [0, 1, 1, 2, 3, 5, 8, 13])

def test_generator_expression():
    result = list(x * 2 for x in range(5))
    test("genexpr_basic", result == [0, 2, 4, 6, 8])
    
    result2 = sum(x * x for x in range(5))
    test("genexpr_sum", result2 == 30)
    
    result3 = list(x for x in range(10) if x % 2 == 0)
    test("genexpr_filter", result3 == [0, 2, 4, 6, 8])

def test_generator_next():
    def gen():
        yield "a"
        yield "b"
        yield "c"
    g = gen()
    test("generator_next_1", next(g) == "a")
    test("generator_next_2", next(g) == "b")
    test("generator_next_3", next(g) == "c")

def test_generator_for_loop():
    def squares(n):
        for i in range(n):
            yield i * i
    result = []
    for x in squares(5):
        result.append(x)
    test("generator_for_loop", result == [0, 1, 4, 9, 16])

def test_generator_tuple():
    def gen():
        yield 10
        yield 20
        yield 30
    test("generator_tuple", tuple(gen()) == (10, 20, 30))

# ── With Statement ──

def test_with_basic():
    class CM:
        def __init__(self):
            self.log = []
        def __enter__(self):
            self.log.append("enter")
            return self
        def __exit__(self, exc_type, exc_val, exc_tb):
            self.log.append("exit")
            return False
    
    cm = CM()
    with cm as c:
        c.log.append("body")
    test("with_basic", cm.log == ["enter", "body", "exit"])

def test_with_exception():
    class SuppressCM:
        def __init__(self):
            self.caught = False
        def __enter__(self):
            return self
        def __exit__(self, exc_type, exc_val, exc_tb):
            if exc_type is not None:
                self.caught = True
                return True
            return False
    
    cm = SuppressCM()
    with cm:
        raise ValueError("suppressed")
    test("with_suppress_exception", cm.caught == True)

def test_with_value():
    class Provider:
        def __enter__(self):
            return 42
        def __exit__(self, *args):
            return False
    
    with Provider() as val:
        result = val
    test("with_value", result == 42)

# ── F-Strings ──

def test_fstring_basic():
    name = "World"
    test("fstring_basic", f"Hello, {name}!" == "Hello, World!")

def test_fstring_expression():
    x = 10
    y = 20
    test("fstring_expr", f"{x + y}" == "30")
    test("fstring_multi", f"{x} + {y} = {x + y}" == "10 + 20 = 30")

def test_fstring_nested():
    items = [1, 2, 3]
    test("fstring_list", f"items: {items}" == "items: [1, 2, 3]")
    test("fstring_len", f"len={len(items)}" == "len=3")

def test_fstring_braces():
    test("fstring_escaped_braces", f"{{x}}" == "{x}")
    test("fstring_literal_braces", f"{{}}" == "{}")

def test_fstring_conversion():
    s = "hello"
    test("fstring_repr", f"{s!r}" == "'hello'")
    test("fstring_str", f"{42!s}" == "42")

def test_fstring_empty():
    test("fstring_no_expr", f"plain text" == "plain text")

# ── Star Unpacking ──

def test_star_unpack_basic():
    a, *b, c = [1, 2, 3, 4, 5]
    test("star_unpack_middle", a == 1 and b == [2, 3, 4] and c == 5)

def test_star_unpack_first():
    first, *rest = [10, 20, 30, 40]
    test("star_unpack_first", first == 10 and rest == [20, 30, 40])

def test_star_unpack_last():
    *init, last = [1, 2, 3]
    test("star_unpack_last", init == [1, 2] and last == 3)

def test_star_unpack_empty():
    a, *b, c = [1, 2]
    test("star_unpack_empty_star", a == 1 and b == [] and c == 2)

def test_star_unpack_tuple():
    a, *b = (10, 20, 30)
    test("star_unpack_tuple", a == 10 and b == [20, 30])

# ── Except As ──

def test_except_as():
    try:
        raise ValueError("test error")
    except ValueError as e:
        msg = str(e)
    test("except_as_message", msg == "test error")

def test_except_as_type():
    caught_type = None
    try:
        x = 1 / 0
    except ZeroDivisionError as e:
        caught_type = "zero_div"
    test("except_as_type", caught_type == "zero_div")

# ── Finally ──

def test_finally_basic():
    result = []
    try:
        result.append("try")
    finally:
        result.append("finally")
    test("finally_basic", result == ["try", "finally"])

def test_finally_with_except():
    result = []
    try:
        result.append("try")
        raise ValueError("err")
    except ValueError:
        result.append("except")
    finally:
        result.append("finally")
    test("finally_with_except", result == ["try", "except", "finally"])

def test_finally_no_exception():
    result = []
    try:
        result.append("try")
    except ValueError:
        result.append("except")
    finally:
        result.append("finally")
    test("finally_no_exception", result == ["try", "finally"])

# ── Import / Modules ──

def test_import_math():
    import math
    test("math_sqrt", math.sqrt(25) == 5.0)
    test("math_pi", abs(math.pi - 3.14159265) < 0.001)
    test("math_ceil", math.ceil(2.3) == 3)
    test("math_floor", math.floor(2.7) == 2)
    test("math_gcd", math.gcd(12, 8) == 4)
    test("math_factorial", math.factorial(6) == 720)
    test("math_isnan", math.isnan(float('nan')) == True)
    test("math_pow", math.pow(2, 10) == 1024.0)
    test("math_log", abs(math.log(math.e) - 1.0) < 0.0001)
    test("math_sin", abs(math.sin(0)) < 0.0001)
    test("math_cos", abs(math.cos(0) - 1.0) < 0.0001)

def test_import_sys():
    import sys
    test("sys_version", "ferrython" in sys.version)
    test("sys_maxsize", sys.maxsize > 0)

def test_import_os():
    import os
    test("os_name", os.name == "posix" or os.name == "nt")
    test("os_sep", os.sep == "/" or os.sep == "\\")
    cwd = os.getcwd()
    test("os_getcwd", len(cwd) > 0)
    pid = os.getpid()
    test("os_getpid", pid > 0)

def test_import_json():
    import json
    s = json.dumps({"a": 1, "b": [2, 3]})
    test("json_dumps", '"a"' in s and '"b"' in s)
    
    obj = json.loads('{"name": "test", "value": 42}')
    test("json_loads_str", obj["name"] == "test")
    test("json_loads_int", obj["value"] == 42)
    
    obj2 = json.loads('[1, 2, 3]')
    test("json_loads_array", obj2 == [1, 2, 3])
    
    test("json_loads_bool", json.loads("true") == True)
    test("json_loads_null", json.loads("null") == None)

def test_import_string():
    import string
    test("string_ascii_lower", string.ascii_lowercase == "abcdefghijklmnopqrstuvwxyz")
    test("string_digits", string.digits == "0123456789")

def test_import_time():
    import time
    t = time.time()
    test("time_time", t > 1000000000)

# ── Run all tests ──

test_generator_basic()
test_generator_range()
test_generator_fibonacci()
test_generator_expression()
test_generator_next()
test_generator_for_loop()
test_generator_tuple()

test_with_basic()
test_with_exception()
test_with_value()

test_fstring_basic()
test_fstring_expression()
test_fstring_nested()
test_fstring_braces()
test_fstring_conversion()
test_fstring_empty()

test_star_unpack_basic()
test_star_unpack_first()
test_star_unpack_last()
test_star_unpack_empty()
test_star_unpack_tuple()

test_except_as()
test_except_as_type()

test_finally_basic()
test_finally_with_except()
test_finally_no_exception()

test_import_math()
test_import_sys()
test_import_os()
test_import_json()
test_import_string()
test_import_time()

print("========================================")
print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(f"SOME TESTS FAILED: {failed}")
