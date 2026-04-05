# Phase 69: Exception chaining, with statement exception handling, str.removeprefix/removesuffix

passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL:", name, "got:", got, "expected:", expected)

# ── Task 1: Exception chaining ──

# Test raise X from Y sets __cause__
cause_set = False
cause_is_correct = False
suppress_ctx = False
try:
    try:
        raise ValueError("original")
    except ValueError as orig:
        raise RuntimeError("chained") from orig
except RuntimeError as e:
    cause_set = e.__cause__ is not None
    cause_is_correct = str(e.__cause__) == "original"
    suppress_ctx = e.__suppress_context__

check("raise_from_cause_set", cause_set, True)
check("raise_from_cause_msg", cause_is_correct, True)
check("raise_from_suppress_ctx", suppress_ctx, True)

# Test raise X from None sets __cause__ to None and __suppress_context__ to True
from_none_cause = "not_none"
from_none_suppress = False
try:
    try:
        raise ValueError("original")
    except ValueError:
        raise RuntimeError("no cause") from None
except RuntimeError as e:
    from_none_cause = e.__cause__
    from_none_suppress = e.__suppress_context__

check("raise_from_none_cause", from_none_cause, None)
check("raise_from_none_suppress", from_none_suppress, True)

# Test implicit chaining sets __context__
context_set = False
context_is_correct = False
try:
    try:
        raise ZeroDivisionError("divide")
    except ZeroDivisionError:
        raise ValueError("new error")
except ValueError as e:
    context_set = e.__context__ is not None
    context_is_correct = str(e.__context__) == "divide"

check("implicit_chain_context_set", context_set, True)
check("implicit_chain_context_msg", context_is_correct, True)

# ── Task 2: With statement exception handling ──

# Test __exit__ receives (None, None, None) on normal exit
class RecordExit:
    def __init__(self):
        self.exit_args = None
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.exit_args = (exc_type, exc_val, exc_tb)
        return False

cm = RecordExit()
with cm:
    x = 42

check("with_normal_exit_type", cm.exit_args[0], None)
check("with_normal_exit_val", cm.exit_args[1], None)
check("with_normal_exit_tb", cm.exit_args[2], None)

# Test with statement exception suppression
class SuppressAll:
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        return True

suppressed = True
try:
    with SuppressAll():
        raise ValueError("suppress me")
    suppressed = True
except ValueError:
    suppressed = False

check("with_suppress_exception", suppressed, True)

# Test with statement exception propagation
class DontSuppress:
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        return False

propagated = False
try:
    with DontSuppress():
        raise ValueError("propagate me")
except ValueError:
    propagated = True

check("with_propagate_exception", propagated, True)

# Test __exit__ receives exception info when exception occurs
class RecordException:
    def __init__(self):
        self.got_exc = False
        self.exc_msg = None
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_val is not None:
            self.got_exc = True
            self.exc_msg = str(exc_val)
        return True

cm2 = RecordException()
with cm2:
    raise ValueError("caught by exit")

check("with_exit_got_exc", cm2.got_exc, True)
check("with_exit_exc_msg", cm2.exc_msg, "caught by exit")

# ── Task 3: str.removeprefix / str.removesuffix ──

check("removeprefix_match", "HelloWorld".removeprefix("Hello"), "World")
check("removeprefix_no_match", "HelloWorld".removeprefix("Foo"), "HelloWorld")
check("removeprefix_empty", "HelloWorld".removeprefix(""), "HelloWorld")
check("removeprefix_full", "Hello".removeprefix("Hello"), "")

check("removesuffix_match", "HelloWorld".removesuffix("World"), "Hello")
check("removesuffix_no_match", "HelloWorld".removesuffix("Foo"), "HelloWorld")
check("removesuffix_empty", "HelloWorld".removesuffix(""), "HelloWorld")
check("removesuffix_full", "Hello".removesuffix("Hello"), "")

print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed > 0:
    raise Exception("TESTS FAILED: " + str(failed))
print("ALL TESTS PASSED!")
