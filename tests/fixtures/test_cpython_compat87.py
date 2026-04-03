## test_cpython_compat87.py - Context managers (~40 tests)
import contextlib

passed87 = 0
total87 = 0

def check87(desc, got, expected):
    global passed87, total87
    total87 += 1
    if got == expected:
        passed87 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- Basic custom context manager ---
class CM1:
    def __init__(self, log):
        self.log = log
    def __enter__(self):
        self.log.append("enter")
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.log.append("exit")
        return False

log1 = []
with CM1(log1) as c1:
    log1.append("body")
check87("basic CM enter/body/exit", log1, ["enter", "body", "exit"])
check87("CM __enter__ returns self", isinstance(c1, CM1), True)

# --- __enter__ returning a different value ---
class CM2:
    def __enter__(self):
        return 42
    def __exit__(self, *args):
        return False

with CM2() as val2:
    pass
check87("CM __enter__ returns 42", val2, 42)

# --- __exit__ suppressing exception ---
class CM3:
    def __init__(self, suppress):
        self.suppress = suppress
        self.caught = None
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.caught = exc_type
        return self.suppress

cm3a = CM3(True)
with cm3a:
    raise ValueError("test")
check87("CM3 suppresses ValueError", cm3a.caught, ValueError)

cm3b = CM3(False)
caught3b = False
try:
    with cm3b:
        raise TypeError("test2")
except TypeError:
    caught3b = True
check87("CM3 does not suppress TypeError", caught3b, True)
check87("CM3 caught TypeError type", cm3b.caught, TypeError)

# --- __exit__ receives exception info ---
class CM4:
    def __init__(self):
        self.exc_info = None
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.exc_info = (exc_type, str(exc_val))
        return True

cm4 = CM4()
with cm4:
    raise RuntimeError("hello")
check87("CM4 exc_type is RuntimeError", cm4.exc_info[0], RuntimeError)
check87("CM4 exc_val message", cm4.exc_info[1], "hello")

# --- __exit__ with no exception ---
class CM5:
    def __init__(self):
        self.args = None
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.args = (exc_type, exc_val, exc_tb)
        return False

cm5 = CM5()
with cm5:
    pass
check87("CM5 no exc: exc_type is None", cm5.args[0], None)
check87("CM5 no exc: exc_val is None", cm5.args[1], None)
check87("CM5 no exc: exc_tb is None", cm5.args[2], None)

# --- Nested with statements ---
log6 = []
class CM6:
    def __init__(self, name, log):
        self.name = name
        self.log = log
    def __enter__(self):
        self.log.append(self.name + "-enter")
        return self
    def __exit__(self, *args):
        self.log.append(self.name + "-exit")
        return False

with CM6("outer", log6) as o6:
    with CM6("inner", log6) as i6:
        log6.append("body")
check87("nested with order", log6, ["outer-enter", "inner-enter", "body", "inner-exit", "outer-exit"])

# --- Nested with, inner raises, outer suppresses ---
log7 = []
class CM7:
    def __init__(self, name, log, suppress):
        self.name = name
        self.log = log
        self.suppress = suppress
    def __enter__(self):
        self.log.append(self.name + "-enter")
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.log.append(self.name + "-exit")
        if exc_type is not None:
            self.log.append(self.name + "-caught")
        return self.suppress

with CM7("outer7", log7, True):
    with CM7("inner7", log7, False):
        raise KeyError("x")
check87("nested exc propagation", "outer7-caught" in log7, True)
check87("nested inner sees exc", "inner7-caught" in log7, True)

# --- contextlib.suppress ---
caught8 = False
with contextlib.suppress(ValueError, TypeError):
    raise ValueError("suppressed")
check87("contextlib.suppress ValueError", True, True)

caught8b = False
try:
    with contextlib.suppress(ValueError):
        raise KeyError("not suppressed")
except KeyError:
    caught8b = True
check87("contextlib.suppress does not catch KeyError", caught8b, True)

# --- contextlib.suppress with no exception ---
result9 = "ok"
with contextlib.suppress(Exception):
    result9 = "done"
check87("contextlib.suppress no exception", result9, "done")

# --- @contextmanager decorator ---
@contextlib.contextmanager
def cm10(log):
    log.append("before")
    yield "value10"
    log.append("after")

log10 = []
with cm10(log10) as v10:
    log10.append("body")
check87("contextmanager yield value", v10, "value10")
check87("contextmanager order", log10, ["before", "body", "after"])

# --- @contextmanager with exception ---
@contextlib.contextmanager
def cm11(log):
    log.append("setup")
    try:
        yield
    except ValueError:
        log.append("caught-ve")
    finally:
        log.append("cleanup")

log11 = []
with cm11(log11):
    raise ValueError("test")
check87("contextmanager catches ValueError", log11, ["setup", "caught-ve", "cleanup"])

# --- @contextmanager cleanup on no exception ---
log12 = []
with cm11(log12):
    log12.append("body")
check87("contextmanager cleanup no exc", log12, ["setup", "body", "cleanup"])

# --- Multiple context managers in single with ---
log13 = []
with CM6("a", log13) as a13, CM6("b", log13) as b13:
    log13.append("body")
check87("multi-with enter order", log13[:2], ["a-enter", "b-enter"])
check87("multi-with exit order", log13[3:], ["b-exit", "a-exit"])
check87("multi-with body", log13[2], "body")

# --- CM that modifies return value ---
class CM14:
    def __enter__(self):
        return [1, 2, 3]
    def __exit__(self, *args):
        return False

with CM14() as lst14:
    lst14.append(4)
check87("CM returns mutable list", lst14, [1, 2, 3, 4])

# --- __exit__ return truthy values ---
class CM15:
    def __init__(self, ret):
        self.ret = ret
    def __enter__(self):
        return self
    def __exit__(self, *args):
        return self.ret

ok15a = True
with CM15(1):
    raise ValueError("x")
check87("__exit__ returning 1 suppresses", True, True)

ok15b = True
with CM15("yes"):
    raise ValueError("x")
check87("__exit__ returning string suppresses", True, True)

ok15c = False
try:
    with CM15(0):
        raise ValueError("x")
except ValueError:
    ok15c = True
check87("__exit__ returning 0 does not suppress", ok15c, True)

ok15d = False
try:
    with CM15(""):
        raise ValueError("x")
except ValueError:
    ok15d = True
check87("__exit__ returning empty string does not suppress", ok15d, True)

ok15e = False
try:
    with CM15(None):
        raise ValueError("x")
except ValueError:
    ok15e = True
check87("__exit__ returning None does not suppress", ok15e, True)

# --- CM __enter__ raising ---
class CM16:
    def __enter__(self):
        raise RuntimeError("enter fail")
    def __exit__(self, *args):
        return False

caught16 = False
try:
    with CM16() as v16:
        pass
except RuntimeError:
    caught16 = True
check87("__enter__ raising prevents body", caught16, True)

# --- CM used without as ---
log17 = []
class CM17:
    def __enter__(self):
        log17.append("enter")
        return "unused"
    def __exit__(self, *args):
        log17.append("exit")
        return False

with CM17():
    log17.append("body")
check87("with without as clause", log17, ["enter", "body", "exit"])

# --- contextmanager yielding None ---
@contextlib.contextmanager
def cm18():
    yield

with cm18() as v18:
    pass
check87("contextmanager yield None", v18, None)

# --- Reusable context manager class ---
class CM19:
    def __init__(self):
        self.count = 0
    def __enter__(self):
        self.count += 1
        return self
    def __exit__(self, *args):
        return False

cm19 = CM19()
with cm19:
    pass
with cm19:
    pass
with cm19:
    pass
check87("reusable CM count", cm19.count, 3)

# --- contextlib.suppress multiple exception types ---
result20 = []
for exc_cls in [ValueError, TypeError, KeyError]:
    with contextlib.suppress(ValueError, TypeError, KeyError):
        result20.append("before")
        raise exc_cls("test")
check87("suppress multiple types count", len(result20), 3)

# --- contextmanager raising in body propagates ---
@contextlib.contextmanager
def cm21():
    yield "val21"

caught21 = False
try:
    with cm21() as v21:
        raise IndexError("oops")
except IndexError:
    caught21 = True
check87("contextmanager propagates unhandled exc", caught21, True)
check87("contextmanager yield value before exc", v21, "val21")

# --- CM exit called even on exception ---
log22 = []
class CM22:
    def __enter__(self):
        log22.append("enter")
        return self
    def __exit__(self, *args):
        log22.append("exit")
        return False

caught22 = False
try:
    with CM22():
        log22.append("body")
        raise RuntimeError("x")
except RuntimeError:
    caught22 = True
check87("exit called on exception", log22, ["enter", "body", "exit"])
check87("exception still raised", caught22, True)

# --- contextmanager yielding mutable ---
@contextlib.contextmanager
def cm23():
    data = [1, 2]
    yield data
    data.append(99)

with cm23() as d23:
    d23.append(3)
check87("contextmanager mutable yield", d23, [1, 2, 3, 99])

print(f"Tests: {total87} | Passed: {passed87} | Failed: {total87 - passed87}")
