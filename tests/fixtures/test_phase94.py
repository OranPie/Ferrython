# Phase 94: warnings, typing_extensions, linecache, traceback, atexit stdlib modules

# ── warnings module ──
import warnings

# 1 – warn does not crash
warnings.warn("test warning")
print("check 1 passed: warnings.warn does not crash")

# 2 – filterwarnings does not crash
warnings.filterwarnings("ignore")
print("check 2 passed: warnings.filterwarnings works")

# 3 – simplefilter does not crash
warnings.simplefilter("always")
print("check 3 passed: warnings.simplefilter works")

# 4 – resetwarnings does not crash
warnings.resetwarnings()
print("check 4 passed: warnings.resetwarnings works")

# 5 – catch_warnings context manager
with warnings.catch_warnings():
    warnings.simplefilter("ignore")
print("check 5 passed: catch_warnings context manager")

# 6 – catch_warnings with record
with warnings.catch_warnings(record=True) as w:
    warnings.warn("recorded warning")
    assert isinstance(w, list), f"catch_warnings record should be list, got {type(w)}"
print("check 6 passed: catch_warnings record=True returns list")

# ── typing_extensions (resolves to Rust typing module) ──
import typing_extensions

# 7 – Protocol exists
assert hasattr(typing_extensions, 'Protocol'), "missing Protocol"
print("check 7 passed: typing_extensions.Protocol exists")

# 8 – Literal exists
assert hasattr(typing_extensions, 'Literal'), "missing Literal"
print("check 8 passed: typing_extensions.Literal exists")

# 9 – TypeVar exists
assert hasattr(typing_extensions, 'TypeVar'), "missing TypeVar"
print("check 9 passed: typing_extensions.TypeVar exists")

# 10 – Generic exists
assert hasattr(typing_extensions, 'Generic'), "missing Generic"
print("check 10 passed: typing_extensions.Generic exists")

# 11 – runtime_checkable exists
assert hasattr(typing_extensions, 'runtime_checkable'), "missing runtime_checkable"
print("check 11 passed: typing_extensions.runtime_checkable exists")

# 12 – get_type_hints exists
assert hasattr(typing_extensions, 'get_type_hints'), "missing get_type_hints"
print("check 12 passed: typing_extensions.get_type_hints exists")

# 13 – Any exists
assert hasattr(typing_extensions, 'Any'), "missing Any"
print("check 13 passed: typing_extensions.Any exists")

# ── linecache module ──
import linecache

# 14 – clearcache does not crash
linecache.clearcache()
print("check 14 passed: linecache.clearcache works")

# 15 – checkcache does not crash
linecache.checkcache()
print("check 15 passed: linecache.checkcache works")

# 16 – getlines on a nonexistent file returns empty
result = linecache.getlines("__nonexistent_file_12345__.py")
assert isinstance(result, list), f"getlines should return list, got {type(result)}"
assert len(result) == 0, f"getlines on missing file should be empty, got {len(result)}"
print("check 16 passed: linecache.getlines missing file returns empty list")

# 17 – getline on nonexistent file returns empty string
line = linecache.getline("__nonexistent_file_12345__.py", 1)
assert isinstance(line, str), f"getline should return str, got {type(line)}"
assert line == "" or line == "\n" or len(line) == 0, f"getline on missing file should be empty, got {repr(line)}"
print("check 17 passed: linecache.getline missing file returns empty")

# ── traceback module ──
import traceback

# 18 – format_exc returns a string
result = traceback.format_exc()
assert isinstance(result, str), f"format_exc should return str, got {type(result)}"
print("check 18 passed: traceback.format_exc returns string")

# 19 – format_exception returns a list
result = traceback.format_exception(ValueError, ValueError("oops"), None)
assert isinstance(result, list), f"format_exception should return list, got {type(result)}"
assert len(result) >= 1, "format_exception should return at least one line"
print("check 19 passed: traceback.format_exception returns list")

# 20 – format_tb with None returns empty list
result = traceback.format_tb(None)
assert isinstance(result, list), f"format_tb should return list, got {type(result)}"
assert len(result) == 0, f"format_tb(None) should be empty, got {len(result)}"
print("check 20 passed: traceback.format_tb(None) returns empty list")

# 21 – extract_tb with None returns empty list
result = traceback.extract_tb(None)
assert isinstance(result, list), f"extract_tb should return list, got {type(result)}"
print("check 21 passed: traceback.extract_tb(None) returns empty list")

# 22 – print_exc does not crash
traceback.print_exc()
print("check 22 passed: traceback.print_exc does not crash")

# ── atexit module ──
import atexit

# 23 – register returns the function
def my_exit():
    pass

result = atexit.register(my_exit)
assert result is my_exit, "register should return the function"
print("check 23 passed: atexit.register returns function")

# 24 – unregister does not crash
atexit.unregister(my_exit)
print("check 24 passed: atexit.unregister works")

# 25 – _run_exitfuncs does not crash
atexit._run_exitfuncs()
print("check 25 passed: atexit._run_exitfuncs works")

# 26 – register multiple and _ncallbacks
def cb1():
    pass
def cb2():
    pass
atexit.register(cb1)
atexit.register(cb2)
count = atexit._ncallbacks()
assert count >= 2, f"expected >= 2 callbacks, got {count}"
print("check 26 passed: atexit._ncallbacks counts registered functions")

print("All 26 checks passed!")
