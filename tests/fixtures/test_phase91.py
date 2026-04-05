# Test phase 91: Enhanced inspect module + re module validation
import inspect
import re

passed = 0
failed = 0

def check(cond, msg):
    global passed, failed
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"FAIL: {msg}")

# ── inspect module ──

def sample_func(x, y, z=10):
    """A sample function for testing."""
    return x + y + z

class MyClass:
    """A sample class."""
    def method(self):
        return 42

# 1. isfunction
check(inspect.isfunction(sample_func), "isfunction(func)")
check(not inspect.isfunction(42), "isfunction(int)")

# 2. isclass
check(inspect.isclass(MyClass), "isclass(class)")
check(not inspect.isclass(sample_func), "isclass(func)")

# 3. ismethod
check(not inspect.ismethod(sample_func), "ismethod(func)")

# 4. isbuiltin
check(inspect.isbuiltin(len), "isbuiltin(len)")
check(not inspect.isbuiltin(sample_func), "isbuiltin(func)")

# 5. signature
sig = inspect.signature(sample_func)
check("x" in str(sig), "signature contains param x")
check("y" in str(sig), "signature contains param y")
check("z" in str(sig), "signature contains param z")

# 6. getfullargspec
spec = inspect.getfullargspec(sample_func)
check("x" in [a if isinstance(a, str) else str(a) for a in spec["args"]], "getfullargspec args contains x")

# 7. getdoc
check(inspect.getdoc(sample_func) == "A sample function for testing.", "getdoc")

# 8. getfile
file_result = inspect.getfile(sample_func)
check("test_phase91" in file_result, "getfile returns filename")

# 9. isroutine
check(inspect.isroutine(sample_func), "isroutine(func)")
check(inspect.isroutine(len), "isroutine(builtin)")

# 10. getmembers
members = inspect.getmembers(MyClass)
member_names = [m[0] for m in members]
check("method" in member_names, "getmembers finds method")

# 11. getmro
mro = inspect.getmro(MyClass)
check(len(mro) >= 1, "getmro returns at least 1")

# ── re module (validate existing) ──

# 12. re.search
m = re.search(r'\d+', 'abc123def')
check(m is not None, "re.search finds digits")
check(m.group() == '123', "re.search group()")

# 13. re.match
m2 = re.match(r'\w+', 'hello world')
check(m2.group() == 'hello', "re.match returns first word")
check(re.match(r'\d+', 'hello') is None, "re.match no match")

# 14. re.findall
found = re.findall(r'\d+', 'a1b2c3')
check(found == ['1', '2', '3'], "re.findall digits")

# 15. re.sub
result = re.sub(r'\d', 'X', 'a1b2c3')
check(result == 'aXbXcX', "re.sub replace digits")

# 16. re.split
parts = re.split(r'[,;]', 'a,b;c,d')
check(parts == ['a', 'b', 'c', 'd'], "re.split on delimiters")

# 17. re.compile
pattern = re.compile(r'(\w+)@(\w+)\.(\w+)')
m3 = pattern.search('user@example.com')
check(m3 is not None, "compiled pattern search")
check(m3.group(1) == 'user', "compiled group(1)")
check(m3.group(2) == 'example', "compiled group(2)")

# 18. re.fullmatch
check(re.fullmatch(r'\d+', '12345') is not None, "fullmatch exact")
check(re.fullmatch(r'\d+', '123abc') is None, "fullmatch no match")

# 19. Match.span()
m4 = re.search(r'world', 'hello world')
start, end = m4.span()
check(start == 6, "match.span() start")
check(end == 11, "match.span() end")

# 20. re.escape
escaped = re.escape('hello.world+foo')
check('\\.' in escaped or r'\.' in escaped, "re.escape dots")

# 21. re.IGNORECASE
m5 = re.search(r'hello', 'HELLO WORLD', re.IGNORECASE)
check(m5 is not None, "IGNORECASE flag")

print(f"test_phase91: {passed} passed, {failed} failed")
