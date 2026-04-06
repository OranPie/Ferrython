import sys

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL: " + name)

# === sys.settrace ===
events = []
def tracer(frame, event, arg):
    events.append((frame.f_code.co_name, event))
    return tracer

def add(a, b):
    return a + b

sys.settrace(tracer)
result = add(1, 2)
sys.settrace(None)
test("settrace_call", any(e == "call" and n == "add" for n, e in events))
test("settrace_return", any(e == "return" and n == "add" for n, e in events))
test("settrace_line", any(e == "line" for _, e in events))
test("settrace_result", result == 3)

# === sys.gettrace ===
sys.settrace(tracer)
test("gettrace_active", sys.gettrace() is not None)
sys.settrace(None)
test("gettrace_none", sys.gettrace() is None)

# === sys.setprofile ===
prof_events = []
def profiler(frame, event, arg):
    prof_events.append((frame.f_code.co_name, event))

sys.setprofile(profiler)
add(3, 4)
sys.setprofile(None)
test("setprofile_call", any(e == "call" and n == "add" for n, e in prof_events))
test("setprofile_return", any(e == "return" and n == "add" for n, e in prof_events))

# === sys.getprofile ===
sys.setprofile(profiler)
test("getprofile_active", sys.getprofile() is not None)
sys.setprofile(None)
test("getprofile_none", sys.getprofile() is None)

# === sys.excepthook ===
test("excepthook_exists", callable(sys.excepthook))
test("__excepthook__exists", callable(sys.__excepthook__))

# === Trace exception event ===
exc_events = []
def exc_tracer(frame, event, arg):
    if event == "exception":
        exc_events.append(("exception", frame.f_code.co_name))
    return exc_tracer

def bad_func():
    raise ValueError("test")

sys.settrace(exc_tracer)
try:
    bad_func()
except ValueError:
    pass
sys.settrace(None)
test("trace_exception", len(exc_events) > 0)

# === Trace frame attributes ===
frame_info = {}
def frame_tracer(frame, event, arg):
    if event == "call":
        frame_info["co_filename"] = frame.f_code.co_filename
        frame_info["co_name"] = frame.f_code.co_name
        frame_info["f_lineno"] = frame.f_lineno
    return frame_tracer

def my_func():
    x = 42
    return x

sys.settrace(frame_tracer)
my_func()
sys.settrace(None)
test("frame_co_name", frame_info.get("co_name") == "my_func")
test("frame_has_lineno", isinstance(frame_info.get("f_lineno"), int))

print(f"\n{passed} passed, {failed} failed out of {passed + failed}")
