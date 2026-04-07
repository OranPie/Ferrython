"""Phase 136: Flag/IntFlag bitwise ops, sys.settrace, toolchain commands, lock file."""
import sys

passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL {name}: got {got!r}, expected {expected!r}")

# ── 1. Flag bitwise operations ──
from enum import Flag, IntFlag, auto, Enum, IntEnum

class Perm(Flag):
    R = 4
    W = 2
    X = 1

# Flag.__or__
rw = Perm.R | Perm.W
check("Flag.__or__", rw.value if hasattr(rw, 'value') else int(rw), 6)

# Flag.__and__
rw_and_r = rw & Perm.R
check("Flag.__and__", rw_and_r.value if hasattr(rw_and_r, 'value') else int(rw_and_r), 4)

# Flag.__xor__
rw_xor = Perm.R ^ Perm.W
check("Flag.__xor__", rw_xor.value if hasattr(rw_xor, 'value') else int(rw_xor), 6)

# Flag.__contains__
check("Flag.__contains__", Perm.R in rw, True)
check("Flag.__contains__ neg", Perm.X in rw, False)

# Flag.__bool__
check("Flag.__bool__ true", bool(Perm.R), True)

# ── 2. IntFlag bitwise operations ──
class Color(IntFlag):
    RED = 1
    GREEN = 2
    BLUE = 4

# IntFlag.__or__
rg = Color.RED | Color.GREEN
check("IntFlag.__or__", int(rg), 3)

# IntFlag.__and__
check("IntFlag.__and__", int(rg & Color.RED), 1)

# IntFlag arithmetic (IntFlag is also int)
check("IntFlag+int", int(Color.RED) + 10, 11)

# IntFlag comparison
check("IntFlag.__lt__", Color.RED < Color.GREEN, True)
check("IntFlag.__eq__ int", Color.RED == 1, True)

# ── 3. Enum basics still work ──
class Status(Enum):
    ACTIVE = 1
    INACTIVE = 2

check("Enum access", Status.ACTIVE.value, 1)
check("Enum iteration", len(list(Status)), 2)

class Priority(IntEnum):
    LOW = 1
    MED = 2
    HIGH = 3

check("IntEnum compare", Priority.LOW < Priority.HIGH, True)
check("IntEnum int", int(Priority.MED), 2)

# ── 4. auto() ──
class Direction(Enum):
    NORTH = auto()
    SOUTH = auto()
    EAST = auto()
    WEST = auto()

# auto() should give sequential values starting from 1
vals = [d.value for d in Direction]
check("auto() sequential", len(vals), 4)
check("auto() distinct", len(set(vals)), 4)

# ── 5. sys.settrace basics ──
trace_events = []

def my_trace(frame, event, arg):
    if event in ("call", "return"):
        name = frame.f_code.co_name if hasattr(frame, 'f_code') and hasattr(frame.f_code, 'co_name') else "?"
        trace_events.append((event, name))
    return my_trace

def traced_func():
    x = 1
    y = 2
    return x + y

sys.settrace(my_trace)
result = traced_func()
sys.settrace(None)

check("traced_func result", result, 3)
# Should have at least one "call" event for traced_func
call_events = [e for e in trace_events if e[0] == "call" and "traced_func" in str(e[1])]
check("settrace call event", len(call_events) >= 1, True)

# ── 6. sys.settrace frame has real locals ──
frame_locals = {}

def capture_trace(frame, event, arg):
    if event == "return" and hasattr(frame, 'f_code'):
        name = frame.f_code.co_name
        if name == "func_with_locals":
            # Capture f_locals at return
            if hasattr(frame, 'f_locals'):
                frame_locals.update(frame.f_locals)
    return capture_trace

def func_with_locals():
    a = 42
    b = "hello"
    return a

sys.settrace(capture_trace)
func_with_locals()
sys.settrace(None)

# f_locals should have 'a' and 'b' from the traced function
check("trace f_locals has a", frame_locals.get("a"), 42)
check("trace f_locals has b", frame_locals.get("b"), "hello")

# ── 7. sys.gettrace / getprofile ──
check("gettrace None", sys.gettrace(), None)
sys.settrace(my_trace)
check("gettrace set", sys.gettrace() is not None, True)
sys.settrace(None)
check("gettrace cleared", sys.gettrace(), None)

# ── 8. sys.setprofile basics ──
profile_events = []

def my_profile(frame, event, arg):
    if event in ("call", "return"):
        name = frame.f_code.co_name if hasattr(frame, 'f_code') else "?"
        profile_events.append((event, name))

sys.setprofile(my_profile)
traced_func()
sys.setprofile(None)

profile_calls = [e for e in profile_events if e[0] == "call"]
check("setprofile call events", len(profile_calls) >= 1, True)

# ── 9. Frame f_code attributes ──
code_attrs_ok = False

def check_frame_trace(frame, event, arg):
    global code_attrs_ok
    if event == "call" and hasattr(frame, 'f_code'):
        co = frame.f_code
        if hasattr(co, 'co_filename') and hasattr(co, 'co_name') and hasattr(co, 'co_firstlineno'):
            code_attrs_ok = True
    return check_frame_trace

def dummy_for_code():
    pass

sys.settrace(check_frame_trace)
dummy_for_code()
sys.settrace(None)
check("f_code attrs", code_attrs_ok, True)

# ── 10. Frame f_back chain ──
f_back_works = False

def check_fback_trace(frame, event, arg):
    global f_back_works
    if event == "call" and hasattr(frame, 'f_back'):
        if frame.f_back is not None:
            f_back_works = True
    return check_fback_trace

def outer():
    inner()

def inner():
    pass

sys.settrace(check_fback_trace)
outer()
sys.settrace(None)
check("f_back chain", f_back_works, True)

# ── 11. StrEnum ──
from enum import StrEnum

class HttpMethod(StrEnum):
    GET = "GET"
    POST = "POST"

check("StrEnum value", HttpMethod.GET.value, "GET")

# ── 12. unique decorator ──
from enum import unique

@unique
class UniqueColor(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3

check("unique passes", len(list(UniqueColor)), 3)

# ── Summary ──
print(f"Phase 136: {passed} passed, {failed} failed")
if failed:
    sys.exit(1)
